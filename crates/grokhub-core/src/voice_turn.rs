//! Voice turns: when someone starts and stops talking, and how a reply is cut
//! into pieces to speak.
//!
//! The cabin keeps one mic stream open for a whole voice session and feeds
//! every block of 16 kHz mono PCM to a [`TurnDetector`]. The detector keeps
//! half a second of audio from before the first word (so a soft start isn't
//! lost), waits out natural pauses, and hands back the whole utterance when the
//! person has finished. Nothing here touches a device or the network.
//!
//! [`SpeechChunker`] takes a reply's text as it streams and hands back pieces
//! ready to synthesize: a short first piece as early as possible, then whole
//! sentences. [`VoiceTimings`] records where a turn's time went.

use std::collections::VecDeque;

/// Sample rate every voice turn runs at (what xAI speech-to-text prefers).
pub const RATE: u32 = 16_000;
/// The detector judges audio in frames this long.
pub const FRAME_MS: u32 = 20;
const FRAME: usize = (RATE * FRAME_MS / 1000) as usize;
/// Audio kept from before the first voiced frame.
pub const PRE_ROLL_MS: u32 = 500;
/// Audio kept after the last voiced frame.
pub const HANGOVER_MS: u32 = 250;
/// Silence that ends a turn.
pub const END_SILENCE_MS: u32 = 900;
/// Silence that ends a short turn, or one whose last sound was a brief burst
/// ("um", a breath before the next clause). People mid-thought pause longer.
pub const PATIENT_END_SILENCE_MS: u32 = 1_400;
/// A turn with less speech than this waits [`PATIENT_END_SILENCE_MS`].
pub const SHORT_TURN_MS: u32 = 1_500;
/// A last burst shorter than this counts as a filler.
pub const FILLER_BURST_MS: u32 = 300;
/// Speech needed before a turn counts (a click or a cough is dropped).
pub const MIN_SPEECH_MS: u32 = 200;
/// Voiced frames in a row that start a turn.
pub const ONSET_MS: u32 = 60;
/// Speech needed to talk over a reply...
pub const BARGE_IN_MS: u32 = 400;
/// ...within this long (the gaps between syllables don't reset it).
pub const BARGE_IN_WINDOW_MS: u32 = 600;
/// The longest turn; it ends here even mid-word.
pub const MAX_TURN_MS: u32 = 60_000;
/// Quiet this long with no turn: a gentle check-in.
pub const CHECK_IN_MS: u32 = 9_000;
/// Quiet this long with no turn: voice pauses itself.
pub const AUTO_PAUSE_MS: u32 = 60_000;

/// A frame is voiced when it is this far above the noise floor.
const MARGIN_DB: f32 = 10.0;
/// Nothing quieter than this is speech, however quiet the room.
const ABS_MIN_DB: f32 = -50.0;
/// The floor is the quietest frame of this window (minimum statistics), so a
/// fan that comes on becomes the floor within it, while the gaps between
/// words keep it down during speech.
const FLOOR_WINDOW_MS: u32 = 5_000;
/// While a reply plays, speech must beat its echo by this much...
const ECHO_MARGIN_DB: f32 = 8.0;
/// ...and nobody can talk over the first half second of it (the echo level is
/// still being learned).
const ECHO_GRACE_MS: u32 = 500;
/// A "turn" this steady (dB spread across its voiced frames) is a machine, not
/// a voice: a hum, a fan, a kettle.
const STEADY_DB: f32 = 1.0;

const fn frames(ms: u32) -> u32 {
    ms / FRAME_MS
}

/// What a block of audio changed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TurnEvent {
    /// Someone is talking (after [`MIN_SPEECH_MS`], or [`BARGE_IN_MS`] over a reply).
    SpeechStarted,
    /// They finished. The whole turn, pre-roll and hangover included, 16 kHz mono.
    TurnEnded(Vec<i16>),
    /// [`CHECK_IN_MS`] of quiet since voice started or the last turn.
    CheckIn,
    /// [`AUTO_PAUSE_MS`] of quiet: time to stop listening.
    AutoPause,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Phase {
    Waiting,
    Talking,
}

/// Finds the start and end of each spoken turn in a live 16 kHz mono stream.
#[derive(Debug, Clone)]
pub struct TurnDetector {
    phase: Phase,
    recent_db: VecDeque<f32>,
    /// Voiced or not, for the last [`BARGE_IN_WINDOW_MS`] (talking over a reply).
    recent_voiced: VecDeque<bool>,
    echo_db: Option<f32>,
    playing_frames: u32,
    voiced_db_sum: f64,
    voiced_db_sq: f64,
    pre_roll: VecDeque<i16>,
    turn: Vec<i16>,
    pending: Vec<i16>,
    onset_run: u32,
    /// Frames since the turn's first voiced frame.
    turn_frames: u32,
    voiced_frames: u32,
    silence_frames: u32,
    burst_frames: u32,
    last_burst_frames: u32,
    announced: bool,
    quiet_frames: u32,
    checked_in: bool,
    paused: bool,
    playing: bool,
}

impl Default for TurnDetector {
    fn default() -> Self {
        Self::new()
    }
}

impl TurnDetector {
    pub fn new() -> Self {
        Self {
            phase: Phase::Waiting,
            recent_db: VecDeque::with_capacity(frames(FLOOR_WINDOW_MS) as usize),
            recent_voiced: VecDeque::with_capacity(frames(BARGE_IN_WINDOW_MS) as usize),
            echo_db: None,
            playing_frames: 0,
            voiced_db_sum: 0.0,
            voiced_db_sq: 0.0,
            pre_roll: VecDeque::with_capacity(samples(PRE_ROLL_MS)),
            turn: Vec::new(),
            pending: Vec::with_capacity(FRAME),
            onset_run: 0,
            turn_frames: 0,
            voiced_frames: 0,
            silence_frames: 0,
            burst_frames: 0,
            last_burst_frames: 0,
            announced: false,
            quiet_frames: 0,
            checked_in: false,
            paused: false,
            playing: false,
        }
    }

    /// True while a turn is under way (someone has started talking).
    pub fn talking(&self) -> bool {
        self.phase == Phase::Talking && self.announced
    }

    /// While a reply plays, only sustained speech well above its echo starts a turn.
    pub fn set_playing(&mut self, playing: bool) {
        if playing != self.playing {
            self.playing_frames = 0;
            self.echo_db = None;
        }
        self.playing = playing;
    }

    /// Start counting quiet again (after a reply, or when voice resumes).
    pub fn reset_quiet(&mut self) {
        self.quiet_frames = 0;
        self.checked_in = false;
        self.paused = false;
    }

    /// Feed the next block of 16 kHz mono samples, any length.
    pub fn push(&mut self, samples: &[i16]) -> Vec<TurnEvent> {
        let mut out = Vec::new();
        for &s in samples {
            self.pending.push(s);
            if self.pending.len() == FRAME {
                let frame = std::mem::take(&mut self.pending);
                self.frame(&frame, &mut out);
                self.pending = frame;
                self.pending.clear();
            }
        }
        out
    }

    /// The person tapped to say they're done: the turn so far, or `None` when
    /// nothing was said. The detector is ready for the next turn either way.
    pub fn finish_now(&mut self) -> Option<Vec<i16>> {
        let said = self.phase == Phase::Talking && self.voiced_frames >= frames(MIN_SPEECH_MS);
        let mut turn = std::mem::take(&mut self.turn);
        turn.extend_from_slice(&self.pending);
        self.pending.clear();
        self.end_turn();
        said.then_some(turn)
    }

    /// Drop any turn in progress (voice left, or a reply was interrupted by tap).
    pub fn clear(&mut self) {
        self.turn.clear();
        self.pending.clear();
        self.end_turn();
    }

    fn end_turn(&mut self) {
        self.phase = Phase::Waiting;
        self.onset_run = 0;
        self.turn_frames = 0;
        self.voiced_frames = 0;
        self.silence_frames = 0;
        self.burst_frames = 0;
        self.last_burst_frames = 0;
        self.announced = false;
        self.voiced_db_sum = 0.0;
        self.voiced_db_sq = 0.0;
        self.pre_roll.clear();
        self.reset_quiet();
    }

    fn floor(&self) -> f32 {
        self.recent_db.iter().copied().fold(f32::INFINITY, f32::min)
    }

    /// Is this frame someone talking?
    fn voiced(&mut self, db: f32) -> bool {
        self.recent_db.push_back(db);
        while self.recent_db.len() > frames(FLOOR_WINDOW_MS) as usize {
            self.recent_db.pop_front();
        }
        let over_floor = db >= (self.floor() + MARGIN_DB).max(ABS_MIN_DB);
        if !self.playing {
            return over_floor;
        }
        self.playing_frames = self.playing_frames.saturating_add(1);
        let echo = *self.echo_db.get_or_insert(db);
        let over_echo = db >= echo + ECHO_MARGIN_DB;
        let grace = self.playing_frames <= frames(ECHO_GRACE_MS);
        // Learn the echo: fast during the grace period, then quick to rise and
        // slow to fall, and only a crawl upward while something beats it.
        let k = match (grace, over_echo, db > echo) {
            (true, _, _) => 0.5,
            (false, true, _) => 0.02,
            (false, false, true) => 0.3,
            (false, false, false) => 0.05,
        };
        self.echo_db = Some(echo + (db - echo) * k);
        over_floor && over_echo && !grace
    }

    fn onset_frames(&self) -> u32 {
        if self.playing {
            frames(BARGE_IN_MS)
        } else {
            frames(ONSET_MS)
        }
    }

    /// Has speech started? Normally a short run of voiced frames; over a reply,
    /// [`BARGE_IN_MS`] of voiced frames within [`BARGE_IN_WINDOW_MS`].
    fn onset(&mut self, voiced: bool) -> bool {
        self.recent_voiced.push_back(voiced);
        while self.recent_voiced.len() > frames(BARGE_IN_WINDOW_MS) as usize {
            self.recent_voiced.pop_front();
        }
        if self.playing {
            let n = self.recent_voiced.iter().filter(|&&v| v).count() as u32;
            n >= frames(BARGE_IN_MS)
        } else {
            self.onset_run >= frames(ONSET_MS)
        }
    }

    fn frame(&mut self, frame: &[i16], out: &mut Vec<TurnEvent>) {
        let db = dbfs(frame);
        let voiced = self.voiced(db);
        if voiced {
            self.voiced_db_sum += f64::from(db);
            self.voiced_db_sq += f64::from(db) * f64::from(db);
        }
        match self.phase {
            Phase::Waiting => self.waiting(frame, voiced, out),
            Phase::Talking => self.talking_frame(frame, voiced, out),
        }
    }

    fn waiting(&mut self, frame: &[i16], voiced: bool, out: &mut Vec<TurnEvent>) {
        if voiced {
            self.onset_run += 1;
        } else {
            self.onset_run = 0;
            self.voiced_db_sum = 0.0;
            self.voiced_db_sq = 0.0;
        }
        // The onset frames themselves live in the pre-roll, which is sized to
        // hold them plus half a second before.
        self.pre_roll.extend(frame.iter().copied());
        let cap = samples(PRE_ROLL_MS) + self.onset_frames() as usize * FRAME;
        while self.pre_roll.len() > cap {
            self.pre_roll.pop_front();
        }
        if self.onset(voiced) {
            let run = self.onset_run.max(1);
            self.recent_voiced.clear();
            self.phase = Phase::Talking;
            self.turn = self.pre_roll.drain(..).collect();
            self.turn_frames = run;
            self.voiced_frames = run;
            self.burst_frames = run;
            self.onset_run = 0;
            if self.playing || run >= frames(MIN_SPEECH_MS) {
                self.announced = true;
                out.push(TurnEvent::SpeechStarted);
            }
            return;
        }
        self.quiet_frames = self.quiet_frames.saturating_add(1);
        if !self.checked_in && self.quiet_frames >= frames(CHECK_IN_MS) {
            self.checked_in = true;
            out.push(TurnEvent::CheckIn);
        }
        if !self.paused && self.quiet_frames >= frames(AUTO_PAUSE_MS) {
            self.paused = true;
            out.push(TurnEvent::AutoPause);
        }
    }

    fn talking_frame(&mut self, frame: &[i16], voiced: bool, out: &mut Vec<TurnEvent>) {
        self.turn.extend_from_slice(frame);
        self.turn_frames += 1;
        if voiced {
            if self.silence_frames > 0 {
                self.burst_frames = 0;
            }
            self.silence_frames = 0;
            self.voiced_frames += 1;
            self.burst_frames += 1;
            if !self.announced && self.voiced_frames >= frames(MIN_SPEECH_MS) {
                self.announced = true;
                out.push(TurnEvent::SpeechStarted);
            }
        } else {
            if self.silence_frames == 0 {
                self.last_burst_frames = self.burst_frames;
            }
            self.silence_frames += 1;
        }
        let capped = self.turn_frames >= frames(MAX_TURN_MS);
        let ended = self.silence_frames >= frames(self.end_silence_ms());
        if !(capped || ended) {
            return;
        }
        if self.voiced_frames < frames(MIN_SPEECH_MS) || self.steady() {
            // A click, a cough, a door, a hum: never a turn.
            self.turn.clear();
            self.end_turn();
            return;
        }
        let mut turn = std::mem::take(&mut self.turn);
        if ended {
            let extra = self.silence_frames.saturating_sub(frames(HANGOVER_MS));
            let keep = turn.len().saturating_sub(extra as usize * FRAME);
            turn.truncate(keep);
        }
        self.end_turn();
        out.push(TurnEvent::TurnEnded(turn));
    }

    /// The turn's voiced frames barely move in level: a machine, not a voice.
    fn steady(&self) -> bool {
        let n = f64::from(self.voiced_frames.max(1));
        if self.voiced_frames < frames(1_000) {
            return false;
        }
        let mean = self.voiced_db_sum / n;
        let var = (self.voiced_db_sq / n - mean * mean).max(0.0);
        (var.sqrt() as f32) < STEADY_DB
    }

    /// How much silence ends this turn.
    fn end_silence_ms(&self) -> u32 {
        let spoken_ms = self.voiced_frames * FRAME_MS;
        let last_burst_ms = self.last_burst_frames * FRAME_MS;
        if spoken_ms < SHORT_TURN_MS || last_burst_ms < FILLER_BURST_MS {
            PATIENT_END_SILENCE_MS
        } else {
            END_SILENCE_MS
        }
    }
}

fn samples(ms: u32) -> usize {
    (RATE * ms / 1000) as usize
}

/// Loudness of a block in dBFS (silence is -120).
pub fn dbfs(samples: &[i16]) -> f32 {
    if samples.is_empty() {
        return -120.0;
    }
    let sum: f64 = samples.iter().map(|&s| f64::from(s) * f64::from(s)).sum();
    let rms = (sum / samples.len() as f64).sqrt();
    if rms < 1.0 {
        -120.0
    } else {
        (20.0 * (rms / 32768.0).log10()) as f32
    }
}

/// Interleaved little-endian s16 bytes to samples (a trailing odd byte is dropped).
pub fn s16le(bytes: &[u8]) -> Vec<i16> {
    bytes
        .as_chunks::<2>()
        .0
        .iter()
        .map(|&b| i16::from_le_bytes(b))
        .collect()
}

/// Any interleaved s16 stream to 16 kHz mono: channels averaged, then
/// linearly resampled. `rate == 16000` and mono pass straight through.
pub fn to_voice_rate(interleaved: &[i16], channels: u16, rate: u32) -> Vec<i16> {
    let ch = usize::from(channels.max(1));
    let mono: Vec<i16> = if ch == 1 {
        interleaved.to_vec()
    } else {
        interleaved
            .chunks_exact(ch)
            .map(|f| (f.iter().map(|&s| i32::from(s)).sum::<i32>() / ch as i32) as i16)
            .collect()
    };
    if rate == RATE || rate == 0 || mono.is_empty() {
        return mono;
    }
    let out_len = (mono.len() as u64 * u64::from(RATE) / u64::from(rate)) as usize;
    let step = f64::from(rate) / f64::from(RATE);
    (0..out_len)
        .map(|i| {
            let pos = i as f64 * step;
            let j = pos as usize;
            let frac = pos - j as f64;
            let a = f64::from(mono[j.min(mono.len() - 1)]);
            let b = f64::from(mono[(j + 1).min(mono.len() - 1)]);
            (a + (b - a) * frac).round() as i16
        })
        .collect()
}

/// A 16-bit mono PCM WAV file around `samples`.
pub fn wav(samples: &[i16], rate: u32) -> Vec<u8> {
    let data_len = (samples.len() * 2) as u32;
    let mut out = Vec::with_capacity(44 + samples.len() * 2);
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(36 + data_len).to_le_bytes());
    out.extend_from_slice(b"WAVEfmt ");
    out.extend_from_slice(&16u32.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&rate.to_le_bytes());
    out.extend_from_slice(&(rate * 2).to_le_bytes());
    out.extend_from_slice(&2u16.to_le_bytes());
    out.extend_from_slice(&16u16.to_le_bytes());
    out.extend_from_slice(b"data");
    out.extend_from_slice(&data_len.to_le_bytes());
    for s in samples {
        out.extend_from_slice(&s.to_le_bytes());
    }
    out
}

/// Recorders that can stream the default mic as endless raw 16 kHz mono s16le,
/// best first (PulseAudio and PipeWire keep the system's own input choice).
pub const MIC_RECORDERS: &[&str] = &["parecord", "pw-record", "ffmpeg", "arecord", "rec"];

/// Arguments for `bin` to stream the default mic to stdout until stopped.
pub fn mic_argv(bin: &str) -> Option<&'static [&'static str]> {
    match bin {
        "parecord" => Some(&[
            "--raw",
            "--format=s16le",
            "--rate=16000",
            "--channels=1",
            "--latency-msec=20",
        ]),
        "pw-record" => Some(&[
            "--rate",
            "16000",
            "--channels",
            "1",
            "--format",
            "s16",
            "--latency",
            "20ms",
            "-",
        ]),
        "ffmpeg" => Some(&[
            "-hide_banner",
            "-loglevel",
            "error",
            "-fflags",
            "nobuffer",
            "-f",
            "pulse",
            "-i",
            "default",
            "-ac",
            "1",
            "-ar",
            "16000",
            "-f",
            "s16le",
            "-",
        ]),
        "arecord" => Some(&[
            "-q", "-t", "raw", "-f", "S16_LE", "-r", "16000", "-c", "1", "-B", "100000",
        ]),
        "rec" => Some(&[
            "-q",
            "-t",
            "raw",
            "-r",
            "16000",
            "-c",
            "1",
            "-b",
            "16",
            "-e",
            "signed-integer",
            "-",
        ]),
        _ => None,
    }
}

/// The first spoken piece goes out at a clause break once it has this many words.
pub const FIRST_CHUNK_WORDS: usize = 6;
/// No spoken piece is longer than this (split at the last comma or space).
pub const MAX_CHUNK_CHARS: usize = 240;

/// Cuts a streaming reply into pieces to speak: the first one as early as a
/// clause allows, then whole sentences. Markdown marks are dropped and code
/// blocks are skipped, so nobody hears "asterisk asterisk".
#[derive(Debug, Clone, Default)]
pub struct SpeechChunker {
    /// Speakable text of finished lines not yet spoken.
    buf: String,
    /// The line still streaming, raw.
    line: String,
    /// How much of the streaming line's speakable text already went out.
    line_spoken: usize,
    sent_first: bool,
    in_code: bool,
    /// Inside text that never reaches the speaker (a thought, a command's body).
    hidden: Option<Hidden>,
}

/// Lines the speaker skips until the block ends.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Hidden {
    /// A `THINKING:` block, up to the next blank line.
    Thought,
    /// A `<think>` block, up to `</think>`.
    ThinkTag,
    /// A command's here-document, up to its end word.
    Until(String),
}

impl SpeechChunker {
    pub fn new() -> Self {
        Self::default()
    }

    /// True once any piece has gone out.
    pub fn started(&self) -> bool {
        self.sent_first
    }

    /// Add streamed text; returns the pieces now ready to speak.
    pub fn push(&mut self, delta: &str) -> Vec<String> {
        let mut out = Vec::new();
        for ch in delta.chars() {
            if ch == '\n' {
                self.end_line(true);
                self.cut(&mut out, false);
            } else {
                self.line.push(ch);
            }
        }
        self.cut(&mut out, true);
        out
    }

    /// The rest of the reply once it has finished streaming.
    pub fn finish(&mut self) -> Option<String> {
        self.end_line(false);
        let piece = clean_piece(&std::mem::take(&mut self.buf));
        self.in_code = false;
        self.hidden = None;
        if piece.is_empty() {
            None
        } else {
            self.sent_first = true;
            Some(piece)
        }
    }

    /// The streaming line's speakable text not yet spoken.
    fn line_rest(&self) -> String {
        if self.in_code
            || self.hidden.is_some()
            || self.line.trim_start().starts_with("```")
            || may_be_hidden(&self.line)
        {
            return String::new();
        }
        let visible = speakable_line(&self.line);
        let at = self.line_spoken.min(visible.len());
        let at = (at..=visible.len())
            .find(|&i| visible.is_char_boundary(i))
            .unwrap_or(visible.len());
        visible[at..].trim_start().to_string()
    }

    fn end_line(&mut self, newline: bool) {
        let line = std::mem::take(&mut self.line);
        let spoken = std::mem::take(&mut self.line_spoken);
        if self.skip_hidden(&line) {
            return;
        }
        if line.trim_start().starts_with("```") {
            self.in_code = !self.in_code;
            return;
        }
        if self.in_code {
            return;
        }
        let visible = speakable_line(&line);
        let rest = visible
            .get(spoken.min(visible.len())..)
            .unwrap_or("")
            .trim_start();
        join_piece(&mut self.buf, rest);
        // A line break ends a list item or a heading: a natural pause.
        if newline && !visible.is_empty() && !self.buf.is_empty() && !ends_sentence(&self.buf) {
            self.buf.push('.');
        }
    }

    /// True when `line` stays off the speaker: inside or opening a thought,
    /// or a command line (and its here-document).
    fn skip_hidden(&mut self, line: &str) -> bool {
        let t = line.trim();
        match &self.hidden {
            Some(Hidden::Thought) => {
                if t.is_empty() {
                    self.hidden = None;
                }
                return true;
            }
            Some(Hidden::ThinkTag) => {
                if t.contains("</think>") {
                    self.hidden = None;
                }
                return true;
            }
            Some(Hidden::Until(end)) => {
                if t == end {
                    self.hidden = None;
                }
                return true;
            }
            None => {}
        }
        if self.in_code {
            return false;
        }
        if t.starts_with("THINKING:") {
            self.hidden = Some(Hidden::Thought);
            return true;
        }
        if t.starts_with("<think>") {
            if !t.contains("</think>") {
                self.hidden = Some(Hidden::ThinkTag);
            }
            return true;
        }
        if crate::chat_view::is_protocol_line(t) {
            self.hidden = crate::chat_view::host_cmd_heredoc_delim(t).map(Hidden::Until);
            return true;
        }
        false
    }

    /// Hand out every ready piece. With `partial`, the streaming line counts too.
    fn cut(&mut self, out: &mut Vec<String>, partial: bool) {
        loop {
            let tail = if partial {
                self.line_rest()
            } else {
                String::new()
            };
            let mut probe = self.buf.clone();
            let before = probe.len();
            join_piece(&mut probe, &tail);
            let Some(at) = self.cut_at(&probe) else {
                return;
            };
            let piece = clean_piece(&probe[..at]);
            if at <= before {
                self.buf = self.buf[at..].trim_start().to_string();
            } else {
                // Part of the streaming line went out too.
                let gap = probe.len() - before - tail.len();
                let used = (at - before).saturating_sub(gap);
                let visible_len = speakable_line(&self.line).len();
                let rest_start = visible_len - tail.len();
                self.line_spoken = rest_start + used;
                self.buf.clear();
            }
            if !piece.is_empty() {
                self.sent_first = true;
                out.push(piece);
            }
        }
    }

    /// Where the next piece ends in `text`, if one is ready.
    fn cut_at(&self, text: &str) -> Option<usize> {
        let words = |s: &str| s.split_whitespace().count();
        let chars: Vec<(usize, char)> = text.char_indices().collect();
        let mut last_soft = None;
        for (k, &(i, c)) in chars.iter().enumerate() {
            let next = chars.get(k + 1).map(|&(_, n)| n);
            let end = i + c.len_utf8();
            let followed_by_space = next.is_some_and(char::is_whitespace);
            if matches!(c, '.' | '!' | '?' | '…') && followed_by_space && !is_abbrev(&text[..i]) {
                return Some(end);
            }
            if matches!(c, ',' | ';' | ':') && followed_by_space {
                last_soft = Some(end);
                if !self.sent_first && words(&text[..end]) >= FIRST_CHUNK_WORDS {
                    return Some(end);
                }
            }
            if end >= MAX_CHUNK_CHARS {
                return last_soft.or_else(|| text[..end].rfind(' ').filter(|&p| p > 0));
            }
        }
        None
    }
}

/// A line still streaming that may yet turn out to be a thought or a command:
/// one of those, or a first word in capitals that has not ended yet.
fn may_be_hidden(line: &str) -> bool {
    let t = line.trim_start();
    if t.starts_with("THINKING") || t.starts_with('<') || crate::chat_view::is_protocol_line(t) {
        return true;
    }
    let first = t.split_whitespace().next().unwrap_or("");
    !first.is_empty()
        && first
            .chars()
            .all(|c| c.is_ascii_uppercase() || c == '_' || c == ':')
        && (first.len() > 2 || !t.contains(' '))
}

fn join_piece(buf: &mut String, piece: &str) {
    if piece.is_empty() {
        return;
    }
    if !buf.is_empty() && !buf.ends_with(char::is_whitespace) {
        buf.push(' ');
    }
    buf.push_str(piece);
}

fn ends_sentence(s: &str) -> bool {
    s.trim_end().ends_with(['.', '!', '?', '…', ':', ';', ','])
}

fn is_abbrev(before: &str) -> bool {
    let word = before
        .rsplit(char::is_whitespace)
        .next()
        .unwrap_or("")
        .to_ascii_lowercase();
    matches!(
        word.as_str(),
        "mr" | "mrs" | "ms" | "dr" | "st" | "vs" | "e.g" | "i.e" | "etc" | "no" | "approx"
    ) || (word.len() == 1 && word.chars().all(|c| c.is_ascii_alphabetic()))
}

/// A line as it should sound: list marks, heading hashes, emphasis and link
/// targets gone; a bare URL becomes "a link".
fn speakable_line(line: &str) -> String {
    let mut t = line.trim();
    t = t.trim_start_matches('#').trim_start();
    for mark in ["- ", "* ", "+ ", "> "] {
        if let Some(rest) = t.strip_prefix(mark) {
            t = rest;
        }
    }
    let digits = t.chars().take_while(char::is_ascii_digit).count();
    if digits > 0 && t[digits..].starts_with(". ") {
        t = &t[digits + 2..];
    }
    let mut out = String::with_capacity(t.len());
    let mut rest = t;
    while let Some(open) = rest.find('[') {
        let after = &rest[open + 1..];
        match (after.find("]("), after.find(']')) {
            (Some(close), Some(first)) if close == first => {
                let tail = &after[close + 2..];
                let Some(paren) = tail.find(')') else { break };
                out.push_str(&rest[..open]);
                out.push_str(&after[..close]);
                rest = &tail[paren + 1..];
            }
            _ => break,
        }
    }
    out.push_str(rest);
    let out: String = out
        .chars()
        .filter(|c| !matches!(c, '*' | '`' | '_' | '|'))
        .collect();
    out.split_whitespace()
        .map(|w| {
            if w.starts_with("http://") || w.starts_with("https://") {
                "a link"
            } else {
                w
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn clean_piece(s: &str) -> String {
    let t = s.trim();
    if t.chars().any(char::is_alphanumeric) {
        t.to_string()
    } else {
        String::new()
    }
}

/// Where one voice turn's time went, in ms after the person stopped talking.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct VoiceTimings {
    /// The turn was declared over (end-of-speech detection).
    pub turn_ms: Option<u64>,
    /// The words came back from speech-to-text.
    pub transcript_ms: Option<u64>,
    /// The reply's first token arrived.
    pub first_token_ms: Option<u64>,
    /// The first reply audio started playing.
    pub first_audio_ms: Option<u64>,
}

impl VoiceTimings {
    /// One log line, e.g. `voice turn: end 900 ms, words 1400, first token 2100, first audio 2700`.
    pub fn line(&self) -> String {
        let f = |v: Option<u64>| v.map_or_else(|| "-".to_string(), |ms| ms.to_string());
        format!(
            "voice turn: end {} ms, words {}, first token {}, first audio {}",
            f(self.turn_ms),
            f(self.transcript_ms),
            f(self.first_token_ms),
            f(self.first_audio_ms)
        )
    }
}

/// The p50 and p95 of each stage across `turns` (`None` where no turn has one).
pub fn timing_percentiles(turns: &[VoiceTimings]) -> [(Option<u64>, Option<u64>); 4] {
    let pick = |f: fn(&VoiceTimings) -> Option<u64>| {
        let mut v: Vec<u64> = turns.iter().filter_map(f).collect();
        if v.is_empty() {
            return (None, None);
        }
        v.sort_unstable();
        let at = |p: usize| v[((v.len() - 1) * p + 50) / 100];
        (Some(at(50)), Some(at(95)))
    };
    [
        pick(|t| t.turn_ms),
        pick(|t| t.transcript_ms),
        pick(|t| t.first_token_ms),
        pick(|t| t.first_audio_ms),
    ]
}

#[cfg(test)]
#[path = "voice_turn_tests.rs"]
mod tests;
