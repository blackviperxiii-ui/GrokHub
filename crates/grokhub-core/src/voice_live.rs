//! Hands-free voice for a shell that owns the mic: one open mic for the whole
//! session, gated into listens, and a streamed reply cut into pieces to speak.
//!
//! Both halves are pure. The shell feeds 16 kHz mono blocks to [`OpenMic`]
//! and acts on what it heard; it feeds reply text to [`ReplyVoice`] and
//! speaks the pieces in order. Threads, devices and speech services stay in
//! the shell.

use crate::voice_turn::{SpeechChunker, TurnDetector, TurnEvent};

/// Reply text past this many bytes is never spoken.
pub const SPOKEN_CAP: usize = crate::attach::TEXT_FILE_CAP;

/// What the open mic heard during one listen.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Heard {
    /// Someone started talking (over a reply: they are talking over it).
    Started,
    /// A finished turn, 16 kHz mono. The listen is over.
    Turn(Vec<i16>),
    /// They tapped done before saying anything. The listen is over.
    Nothing,
    /// Nobody has spoken for a while; still listening.
    CheckIn,
    /// Nobody has spoken for a minute. The listen is over.
    Pause,
}

impl Heard {
    /// True when this ends the listen.
    pub fn ends_listen(&self) -> bool {
        matches!(self, Self::Turn(_) | Self::Nothing | Self::Pause)
    }
}

/// The open mic's gate. Blocks reach the turn detector only while a listen
/// is armed; between listens (thinking, or a reply playing with no listen)
/// they are dropped. Over a playing reply only sustained speech starts a
/// turn, and the reply's length never counts as a quiet minute.
#[derive(Debug, Clone, Default)]
pub struct OpenMic {
    det: TurnDetector,
    armed: bool,
    /// The detector has seen a block of this listen.
    fresh: bool,
    playing: bool,
    finish: bool,
}

impl OpenMic {
    pub fn new() -> Self {
        Self::default()
    }

    /// Start listening for the next turn.
    pub fn listen(&mut self) {
        self.finish = false;
        self.fresh = false;
        self.armed = true;
    }

    /// Stop listening without a turn (voice off, or a reply stopped by tap).
    pub fn stop(&mut self) {
        self.armed = false;
        self.finish = false;
        self.det.clear();
    }

    /// A listen is armed.
    pub fn armed(&self) -> bool {
        self.armed
    }

    /// Someone is mid-turn right now.
    pub fn talking(&self) -> bool {
        self.armed && self.det.talking()
    }

    /// A reply is (or stopped) playing through the speakers.
    pub fn set_playing(&mut self, playing: bool) {
        self.playing = playing;
    }

    /// End the turn at the next block (the person tapped to say they're done).
    pub fn finish(&mut self) {
        self.finish = true;
    }

    /// Feed one block; what it heard, in order.
    pub fn push(&mut self, block: &[i16]) -> Vec<Heard> {
        if !self.armed {
            return Vec::new();
        }
        if !self.fresh {
            // A fresh listen: the room's noise floor carries over, the turn doesn't.
            self.det.clear();
            self.fresh = true;
        }
        self.det.set_playing(self.playing);
        if self.playing {
            // A long reply is not a quiet minute.
            self.det.reset_quiet();
        }
        if std::mem::take(&mut self.finish) {
            self.det.push(block);
            self.armed = false;
            return vec![match self.det.finish_now() {
                Some(turn) => Heard::Turn(turn),
                None => Heard::Nothing,
            }];
        }
        let mut out = Vec::new();
        for ev in self.det.push(block) {
            let heard = match ev {
                TurnEvent::SpeechStarted => Heard::Started,
                TurnEvent::CheckIn => Heard::CheckIn,
                TurnEvent::TurnEnded(turn) => Heard::Turn(turn),
                TurnEvent::AutoPause => Heard::Pause,
            };
            let ends = heard.ends_listen();
            out.push(heard);
            if ends {
                self.armed = false;
                break;
            }
        }
        out
    }
}

/// A streamed reply cut into pieces to speak: the first piece as soon as a
/// clause allows, then sentence by sentence. Never more than [`SPOKEN_CAP`]
/// bytes of text, and nothing at all once hushed.
#[derive(Debug, Clone)]
pub struct ReplyVoice {
    chunker: SpeechChunker,
    left: usize,
    hushed: bool,
    finished: bool,
}

impl Default for ReplyVoice {
    fn default() -> Self {
        Self::new()
    }
}

impl ReplyVoice {
    pub fn new() -> Self {
        Self {
            chunker: SpeechChunker::new(),
            left: SPOKEN_CAP,
            hushed: false,
            finished: false,
        }
    }

    /// More of the reply as it streams; the pieces now ready to speak.
    pub fn feed(&mut self, delta: &str) -> Vec<String> {
        if self.hushed || self.finished {
            return Vec::new();
        }
        let delta = self.within_cap(delta);
        self.chunker.push(delta)
    }

    /// The reply is complete: the rest to speak. `whole` is spoken when
    /// nothing streamed (a reply that came in one piece).
    pub fn finish(&mut self, whole: &str) -> Vec<String> {
        if self.hushed || self.finished {
            return Vec::new();
        }
        self.finished = true;
        let mut out = Vec::new();
        if !self.chunker.started() {
            let whole = self.within_cap(whole);
            out.extend(self.chunker.push(whole));
        }
        // Ending the last line cuts it into sentences like the rest.
        out.extend(self.chunker.push("\n"));
        out.extend(self.chunker.finish());
        out
    }

    /// Say no more of this reply, even as it keeps streaming in.
    pub fn hush(&mut self) {
        self.hushed = true;
        self.left = 0;
    }

    /// True once any piece has gone out.
    pub fn started(&self) -> bool {
        self.chunker.started()
    }

    /// The part of `text` that still fits the cap.
    fn within_cap<'a>(&mut self, text: &'a str) -> &'a str {
        let mut end = self.left.min(text.len());
        while !text.is_char_boundary(end) {
            end -= 1;
        }
        self.left -= end;
        &text[..end]
    }
}

#[cfg(test)]
#[path = "voice_live_tests.rs"]
mod tests;
