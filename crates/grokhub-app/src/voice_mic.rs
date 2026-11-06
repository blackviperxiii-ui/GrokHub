//! One open mic for a whole voice session.
//!
//! The mic opens once when voice mode starts and stays open until it ends, so
//! the first word of each turn is never lost to a device opening. A capture
//! thread feeds 16 kHz mono blocks to the turn detector only while a listen is
//! armed; between turns (thinking, speaking) the blocks are dropped.

use std::io::Read;
use std::process::{Child, ChildStdout, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};

use grokhub_core::voice_turn::{self as vt, TurnDetector, TurnEvent};

/// What the open mic heard during one listen.
#[derive(Debug)]
pub enum Heard {
    /// Someone started talking.
    Started,
    /// A finished turn, 16 kHz mono.
    Turn(Vec<i16>),
    /// They tapped done before saying anything.
    Nothing,
    /// Nobody has spoken for a while; still listening.
    CheckIn,
    /// Nobody has spoken for a minute; listening stopped.
    Pause,
    /// The mic stopped or never opened.
    Lost(String),
}

#[derive(Default)]
struct Shared {
    armed: AtomicBool,
    finish: AtomicBool,
    stop: AtomicBool,
    talking: AtomicBool,
    out: Mutex<Option<Sender<Heard>>>,
    /// A copy of each block heard during the listen (streaming speech-to-text).
    tap: Mutex<Option<Sender<Vec<i16>>>>,
    lost: Mutex<Option<String>>,
    child: Mutex<Option<Child>>,
}

impl Shared {
    fn send(&self, heard: Heard) {
        if let Some(tx) = self.out.lock().unwrap_or_else(|e| e.into_inner()).as_ref() {
            let _ = tx.send(heard);
        }
    }

    /// The listen is over: no more blocks to the detector, no more messages.
    fn disarm_with(&self, heard: Heard) {
        self.armed.store(false, Ordering::SeqCst);
        self.talking.store(false, Ordering::SeqCst);
        // Closing the copy tells the stream the turn is over.
        self.tap.lock().unwrap_or_else(|e| e.into_inner()).take();
        let tx = self.out.lock().unwrap_or_else(|e| e.into_inner()).take();
        if let Some(tx) = tx {
            let _ = tx.send(heard);
        }
    }

    fn lose(&self, why: String) {
        *self.lost.lock().unwrap_or_else(|e| e.into_inner()) = Some(why.clone());
        self.disarm_with(Heard::Lost(why));
    }

    fn kill_child(&self) {
        let child = self.child.lock().unwrap_or_else(|e| e.into_inner()).take();
        if let Some(mut child) = child {
            crate::desktop::kill_limited(&mut child);
            let _ = child.wait();
        }
    }
}

/// The voice session's mic. Dropping it closes the mic.
pub struct VoiceMic {
    shared: Arc<Shared>,
}

impl VoiceMic {
    /// Start opening the mic. Never blocks: a mic that can't open reports
    /// [`Heard::Lost`] on the first listen.
    pub fn open() -> Self {
        let shared = Arc::new(Shared::default());
        let worker = shared.clone();
        let spawned = std::thread::Builder::new()
            .name("voice-mic".into())
            .spawn(move || capture(worker));
        if let Err(e) = spawned {
            shared.lose(format!("the mic thread didn't start: {e}"));
        }
        Self { shared }
    }

    /// Listen for the next turn. Messages arrive on the returned channel until
    /// a turn, a pause or a lost mic ends the listen. With a `tap`, a copy of
    /// every block heard goes there too, until the listen ends.
    pub fn listen(&self, tap: Option<Sender<Vec<i16>>>) -> Receiver<Heard> {
        let (tx, rx) = mpsc::channel();
        let lost = self
            .shared
            .lost
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone();
        if let Some(why) = lost {
            let _ = tx.send(Heard::Lost(why));
            return rx;
        }
        self.shared.finish.store(false, Ordering::SeqCst);
        *self.shared.tap.lock().unwrap_or_else(|e| e.into_inner()) = tap;
        *self.shared.out.lock().unwrap_or_else(|e| e.into_inner()) = Some(tx);
        self.shared.armed.store(true, Ordering::SeqCst);
        rx
    }

    /// Someone is mid-turn right now.
    pub fn talking(&self) -> bool {
        self.shared.armed.load(Ordering::SeqCst) && self.shared.talking.load(Ordering::SeqCst)
    }

    /// End the turn now (the person tapped to say they're done).
    pub fn finish(&self) {
        self.shared.finish.store(true, Ordering::SeqCst);
    }

    /// A handle that can end the current turn from another thread (the
    /// stream's Smart Turn decided the person is done).
    pub fn finisher(&self) -> MicFinisher {
        MicFinisher(self.shared.clone())
    }
}

/// Ends the mic's current turn; see [`VoiceMic::finisher`].
#[derive(Clone)]
pub struct MicFinisher(Arc<Shared>);

impl MicFinisher {
    pub fn finish(&self) {
        self.0.finish.store(true, Ordering::SeqCst);
    }

    /// One tied to no mic, and whether it was asked to finish.
    #[cfg(test)]
    pub fn detached() -> Self {
        Self(Arc::default())
    }

    #[cfg(test)]
    pub fn asked(&self) -> bool {
        self.0.finish.load(Ordering::SeqCst)
    }
}

impl Drop for VoiceMic {
    fn drop(&mut self) {
        self.shared.stop.store(true, Ordering::SeqCst);
        self.shared.armed.store(false, Ordering::SeqCst);
        self.shared
            .out
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .take();
        self.shared
            .tap
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .take();
        // A recorder blocked mid-read ends its pipe when killed.
        self.shared.kill_child();
    }
}

/// Where 16 kHz mono blocks come from.
enum Source {
    Pipe(ChildStdout),
    #[cfg(windows)]
    Wave(crate::win_audio::WaveIn),
}

impl Source {
    fn open(shared: &Shared) -> Result<Self, String> {
        #[cfg(windows)]
        {
            if let Some(mic) = crate::win_audio::WaveIn::start() {
                return Ok(Self::Wave(mic));
            }
        }
        let bin = vt::MIC_RECORDERS
            .iter()
            .find(|b| crate::desktop::which(b))
            .ok_or_else(no_recorder)?;
        let argv = vt::mic_argv(bin).ok_or_else(no_recorder)?;
        let mut cmd = crate::desktop::spawn_bin(bin);
        cmd.args(argv)
            .env("LC_ALL", "C")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        crate::host::hide_windows_console(&mut cmd);
        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt;
            cmd.process_group(0);
        }
        let mut child = cmd
            .spawn()
            .map_err(|e| format!("{bin} didn't start: {e}"))?;
        let out = child.stdout.take().ok_or("the recorder gave no output")?;
        *shared.child.lock().unwrap_or_else(|e| e.into_inner()) = Some(child);
        Ok(Self::Pipe(out))
    }

    /// The next block of 16 kHz mono samples (empty when the mic is idle).
    fn read(&mut self, buf: &mut [u8]) -> Result<Vec<i16>, String> {
        match self {
            Self::Pipe(out) => match out.read(buf) {
                Ok(0) => Err("the mic stopped".into()),
                Ok(n) => Ok(vt::s16le(&buf[..n & !1])),
                Err(e) => Err(format!("the mic stopped: {e}")),
            },
            #[cfg(windows)]
            Self::Wave(mic) => match mic.read_frame() {
                Some(pcm) => Ok(vt::to_voice_rate(
                    &vt::s16le(&pcm),
                    1,
                    crate::win_audio::RATE,
                )),
                None => Err("the Windows mic stopped".into()),
            },
        }
    }
}

fn no_recorder() -> String {
    "No microphone recorder found: install pulseaudio-utils (parecord), pipewire (pw-record), ffmpeg or alsa-utils (arecord).".into()
}

/// The capture thread: open the mic, then feed armed listens until dropped.
fn capture(shared: Arc<Shared>) {
    let mut source = match Source::open(&shared) {
        Ok(s) => s,
        Err(e) => {
            shared.lose(e);
            return;
        }
    };
    let mut det = TurnDetector::new();
    let mut was_armed = false;
    // 100 ms of 16 kHz s16le per pipe read.
    let mut buf = vec![0u8; 3200];
    while !shared.stop.load(Ordering::SeqCst) {
        let block = match source.read(&mut buf) {
            Ok(b) => b,
            Err(e) => {
                if !shared.stop.load(Ordering::SeqCst) {
                    shared.lose(e);
                }
                break;
            }
        };
        step(&shared, &mut det, &mut was_armed, &block);
    }
    shared.kill_child();
}

/// Hand one block to the detector when a listen is armed.
fn step(shared: &Shared, det: &mut TurnDetector, was_armed: &mut bool, block: &[i16]) {
    if !shared.armed.load(Ordering::SeqCst) {
        *was_armed = false;
        return;
    }
    if !*was_armed {
        // A fresh listen: the room's noise floor carries over, the turn doesn't.
        det.clear();
        *was_armed = true;
    }
    if let Some(tap) = shared
        .tap
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .as_ref()
    {
        let _ = tap.send(block.to_vec());
    }
    if shared.finish.swap(false, Ordering::SeqCst) {
        det.push(block);
        let heard = match det.finish_now() {
            Some(turn) => Heard::Turn(turn),
            None => Heard::Nothing,
        };
        *was_armed = false;
        shared.disarm_with(heard);
        return;
    }
    for ev in det.push(block) {
        match ev {
            TurnEvent::SpeechStarted => {
                shared.talking.store(true, Ordering::SeqCst);
                shared.send(Heard::Started);
            }
            TurnEvent::CheckIn => shared.send(Heard::CheckIn),
            TurnEvent::TurnEnded(turn) => {
                *was_armed = false;
                shared.disarm_with(Heard::Turn(turn));
                return;
            }
            TurnEvent::AutoPause => {
                *was_armed = false;
                shared.disarm_with(Heard::Pause);
                return;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tone(ms: u32, amp: f32) -> Vec<i16> {
        let n = (vt::RATE * ms / 1000) as usize;
        (0..n)
            .map(|i| {
                let t = i as f32 / vt::RATE as f32;
                let env = 0.08 + 0.92 * (2.0 * std::f32::consts::PI * 2.0 * t).sin().abs();
                (amp * env * (2.0 * std::f32::consts::PI * 220.0 * t).sin()) as i16
            })
            .collect()
    }

    fn hush(ms: u32) -> Vec<i16> {
        let n = (vt::RATE * ms / 1000) as usize;
        (0..n).map(|i| ((i * 7919 % 61) as i16) - 30).collect()
    }

    fn feed(shared: &Shared, det: &mut TurnDetector, armed: &mut bool, audio: &[i16]) {
        for block in audio.chunks(1600) {
            step(shared, det, armed, block);
        }
    }

    fn drain(rx: &Receiver<Heard>) -> Vec<Heard> {
        rx.try_iter().collect()
    }

    fn listen(shared: &Shared) -> Receiver<Heard> {
        let (tx, rx) = mpsc::channel();
        *shared.out.lock().unwrap() = Some(tx);
        shared.armed.store(true, Ordering::SeqCst);
        rx
    }

    #[test]
    fn a_long_turn_arrives_whole_and_the_listen_ends() {
        let shared = Shared::default();
        let mut det = TurnDetector::new();
        let mut armed = false;
        let rx = listen(&shared);
        let mut audio = hush(1000);
        audio.extend(tone(7000, 9000.0));
        audio.extend(hush(2000));
        feed(&shared, &mut det, &mut armed, &audio);
        let heard = drain(&rx);
        assert!(matches!(heard.first(), Some(Heard::Started)), "{heard:?}");
        let Some(Heard::Turn(turn)) = heard.last() else {
            panic!("a turn: {heard:?}");
        };
        let ms = turn.len() as u32 * 1000 / vt::RATE;
        assert!(ms >= 7000, "a 7 s turn is not cut at 4 s: {ms} ms");
        assert!(!shared.armed.load(Ordering::SeqCst), "one turn per listen");
        assert!(shared.out.lock().unwrap().is_none());
    }

    #[test]
    fn blocks_between_listens_are_dropped() {
        let shared = Shared::default();
        let mut det = TurnDetector::new();
        let mut armed = false;
        // Speaking while nobody listens (the reply playing) is never a turn.
        feed(&shared, &mut det, &mut armed, &tone(3000, 9000.0));
        let rx = listen(&shared);
        feed(&shared, &mut det, &mut armed, &hush(1500));
        assert!(drain(&rx).is_empty());
        assert!(!det.talking());
    }

    #[test]
    fn a_tap_mid_turn_keeps_what_was_said() {
        let shared = Shared::default();
        let mut det = TurnDetector::new();
        let mut armed = false;
        let rx = listen(&shared);
        let mut audio = hush(800);
        audio.extend(tone(1500, 9000.0));
        feed(&shared, &mut det, &mut armed, &audio);
        assert!(shared.talking.load(Ordering::SeqCst));
        shared.finish.store(true, Ordering::SeqCst);
        feed(&shared, &mut det, &mut armed, &tone(100, 9000.0));
        let heard = drain(&rx);
        let Some(Heard::Turn(turn)) = heard.last() else {
            panic!("the tap hands over the turn: {heard:?}");
        };
        assert!(turn.len() as u32 * 1000 / vt::RATE >= 1500);
        assert!(!shared.talking.load(Ordering::SeqCst));
    }

    #[test]
    fn a_tap_before_any_words_hears_nothing() {
        let shared = Shared::default();
        let mut det = TurnDetector::new();
        let mut armed = false;
        let rx = listen(&shared);
        feed(&shared, &mut det, &mut armed, &hush(600));
        shared.finish.store(true, Ordering::SeqCst);
        feed(&shared, &mut det, &mut armed, &hush(100));
        assert!(matches!(drain(&rx).last(), Some(Heard::Nothing)));
    }

    #[test]
    fn the_stream_gets_every_block_of_the_listen_and_hears_when_it_ends() {
        let shared = Shared::default();
        let mut det = TurnDetector::new();
        let mut armed = false;
        let (tap, copies) = mpsc::channel();
        // Before the listen: nothing is copied.
        feed(&shared, &mut det, &mut armed, &tone(500, 9000.0));
        *shared.tap.lock().unwrap() = Some(tap);
        let rx = listen(&shared);
        let mut audio = hush(500);
        audio.extend(tone(1500, 9000.0));
        audio.extend(hush(2500));
        feed(&shared, &mut det, &mut armed, &audio);
        assert!(matches!(drain(&rx).last(), Some(Heard::Turn(_))));
        let copied: usize = copies.try_iter().map(|b| b.len()).sum();
        assert!(copied > 0 && copied <= audio.len(), "{copied}");
        assert!(
            copies.recv().is_err(),
            "the copy channel closes with the turn, so the stream finalizes"
        );
    }

    #[test]
    fn a_quiet_minute_checks_in_then_pauses() {
        let shared = Shared::default();
        let mut det = TurnDetector::new();
        let mut armed = false;
        let rx = listen(&shared);
        feed(
            &shared,
            &mut det,
            &mut armed,
            &hush(vt::AUTO_PAUSE_MS + 200),
        );
        let heard = drain(&rx);
        assert!(
            heard.iter().any(|h| matches!(h, Heard::CheckIn)),
            "{heard:?}"
        );
        assert!(matches!(heard.last(), Some(Heard::Pause)), "{heard:?}");
    }

    #[test]
    fn a_lost_mic_answers_every_later_listen() {
        let shared = Arc::new(Shared::default());
        let rx = {
            let (tx, rx) = mpsc::channel();
            *shared.out.lock().unwrap() = Some(tx);
            rx
        };
        shared.lose("the mic stopped".into());
        assert!(matches!(rx.try_recv(), Ok(Heard::Lost(_))));
        let mic = VoiceMic {
            shared: shared.clone(),
        };
        assert!(matches!(mic.listen(None).try_recv(), Ok(Heard::Lost(_))));
    }
}
