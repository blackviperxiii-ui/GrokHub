//! Streaming speech-to-text: the socket address, its events, and how a turn's
//! words come together. The app owns the socket; this part is pure.
//!
//! The server's Smart Turn model ends a turn when it is confident the person
//! has finished their thought, and holds off at a mid-sentence pause.

use serde_json::Value;

/// Silence (ms) before the server considers an utterance over.
pub const ENDPOINTING_MS: u32 = 500;
/// Smart Turn's end-of-turn threshold: conservative, so a pause to think
/// isn't taken as done.
pub const SMART_TURN: &str = "0.7";
/// Longest silence (ms) before the server ends a turn even when unsure.
pub const SMART_TURN_TIMEOUT_MS: u32 = 2500;

/// The streaming socket: 16 kHz PCM, interim results, Smart Turn, and the
/// words the app is most likely to hear spelled the way it spells them.
pub fn stt_stream_url(keyterms: &[&str]) -> String {
    let mut url = format!(
        "wss://api.x.ai/v1/stt?sample_rate={}&encoding=pcm&interim_results=true\
         &endpointing={ENDPOINTING_MS}&smart_turn={SMART_TURN}\
         &smart_turn_timeout={SMART_TURN_TIMEOUT_MS}&language=en&format=true",
        crate::voice_turn::RATE
    );
    for term in keyterms {
        let term: String = term.chars().take(50).collect();
        if term.trim().is_empty() {
            continue;
        }
        url.push_str("&keyterm=");
        url.push_str(&query_escape(term.trim()));
    }
    url
}

fn query_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            b' ' => out.push('+'),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

/// Tells the server the person is done now (a tap, or the app's own detector).
pub const FINALIZE: &str = r#"{"type":"finalize"}"#;

/// One server event.
#[derive(Debug, Clone, PartialEq)]
pub enum SttEvent {
    /// Ready for audio.
    Ready,
    /// Words so far. `is_final` locks them; `speech_final` ends the utterance.
    Partial {
        text: String,
        is_final: bool,
        speech_final: bool,
    },
    /// The last words after the audio ended.
    Done(String),
    Error(String),
    Other,
}

pub fn parse_stt_event(raw: &str) -> SttEvent {
    let Ok(v) = serde_json::from_str::<Value>(raw) else {
        return SttEvent::Other;
    };
    let text = || {
        v.get("text")
            .and_then(Value::as_str)
            .unwrap_or("")
            .trim()
            .to_string()
    };
    let flag = |k: &str| v.get(k).and_then(Value::as_bool).unwrap_or(false);
    match v.get("type").and_then(Value::as_str).unwrap_or("") {
        "transcript.created" => SttEvent::Ready,
        "transcript.partial" => SttEvent::Partial {
            text: text(),
            is_final: flag("is_final"),
            speech_final: flag("speech_final"),
        },
        "transcript.done" => SttEvent::Done(text()),
        "error" => SttEvent::Error(
            v.get("message")
                .and_then(Value::as_str)
                .unwrap_or("speech-to-text error")
                .chars()
                .take(200)
                .collect(),
        ),
        _ => SttEvent::Other,
    }
}

/// What one event means for the turn.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TurnStep {
    /// Live caption: everything heard so far.
    Caption(String),
    /// The turn is over; these are its words (may be empty: noise only).
    Ended(String),
    Nothing,
}

/// The words of one turn as events arrive.
#[derive(Debug, Clone, Default)]
pub struct SttTurn {
    /// Locked chunks of an utterance not yet ended.
    locked: String,
    /// Utterances already ended inside this turn (a demoted pause, a cough).
    done: String,
    caption: String,
}

impl SttTurn {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn apply(&mut self, ev: &SttEvent) -> TurnStep {
        match ev {
            SttEvent::Partial {
                text,
                is_final: false,
                ..
            } => self.caption_with(text),
            SttEvent::Partial {
                text,
                is_final: true,
                speech_final: false,
            } => {
                join(&mut self.locked, text);
                self.caption_with("")
            }
            SttEvent::Partial {
                text,
                speech_final: true,
                ..
            } => TurnStep::Ended(self.close_utterance(text)),
            SttEvent::Done(text) => TurnStep::Ended(self.close_utterance(text)),
            _ => TurnStep::Nothing,
        }
    }

    /// Everything said so far, for a fallback that ends the turn itself.
    pub fn heard(&self) -> String {
        let mut all = self.done.clone();
        join(&mut all, &self.locked);
        all
    }

    fn caption_with(&mut self, interim: &str) -> TurnStep {
        let mut c = self.heard();
        join(&mut c, interim);
        if c == self.caption || c.is_empty() {
            return TurnStep::Nothing;
        }
        self.caption = c.clone();
        TurnStep::Caption(c)
    }

    /// The utterance's final text is stitched whole by the server; keep the
    /// locked chunks only if it somehow left them out.
    fn close_utterance(&mut self, text: &str) -> String {
        let locked = std::mem::take(&mut self.locked);
        let mut utterance = text.trim().to_string();
        if !locked.is_empty() && !contains_words(&utterance, &locked) {
            let mut whole = locked;
            join(&mut whole, &utterance);
            utterance = whole;
        }
        join(&mut self.done, &utterance);
        self.done.clone()
    }
}

fn join(buf: &mut String, piece: &str) {
    let piece = piece.trim();
    if piece.is_empty() {
        return;
    }
    if !buf.is_empty() {
        buf.push(' ');
    }
    buf.push_str(piece);
}

/// True when `whole` already holds the first words of `part`.
fn contains_words(whole: &str, part: &str) -> bool {
    let norm = |s: &str| -> String {
        s.chars()
            .filter(|c| c.is_alphanumeric() || c.is_whitespace())
            .collect::<String>()
            .to_lowercase()
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ")
    };
    let head: String = norm(part)
        .split_whitespace()
        .take(3)
        .collect::<Vec<_>>()
        .join(" ");
    head.is_empty() || norm(whole).contains(&head)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn partial(text: &str, is_final: bool, speech_final: bool) -> SttEvent {
        SttEvent::Partial {
            text: text.into(),
            is_final,
            speech_final,
        }
    }

    #[test]
    fn the_url_asks_for_smart_turn_at_16k_with_escaped_keyterms() {
        let url = stt_stream_url(&["Grok", "Loadout Hub", ""]);
        assert!(url.starts_with("wss://api.x.ai/v1/stt?sample_rate=16000&encoding=pcm"));
        for part in [
            "interim_results=true",
            "endpointing=500",
            "smart_turn=0.7",
            "smart_turn_timeout=2500",
            "language=en",
            "keyterm=Grok",
            "keyterm=Loadout+Hub",
        ] {
            assert!(url.contains(part), "{part}: {url}");
        }
        assert_eq!(url.matches("keyterm=").count(), 2, "{url}");
        assert!(stt_stream_url(&["a&b=c"]).contains("keyterm=a%26b%3Dc"));
    }

    #[test]
    fn events_parse_from_the_documented_shapes() {
        assert_eq!(
            parse_stt_event(r#"{"type":"transcript.created"}"#),
            SttEvent::Ready
        );
        assert_eq!(
            parse_stt_event(
                r#"{"type":"transcript.partial","text":"I will buy two","is_final":true,"speech_final":true,"end_of_turn_confidence":0.98}"#
            ),
            partial("I will buy two", true, true)
        );
        assert_eq!(
            parse_stt_event(r#"{"type":"transcript.done","text":" all of it ","duration":2.1}"#),
            SttEvent::Done("all of it".into())
        );
        assert_eq!(
            parse_stt_event(r#"{"type":"error","message":"bad audio"}"#),
            SttEvent::Error("bad audio".into())
        );
        assert_eq!(parse_stt_event("not json"), SttEvent::Other);
    }

    #[test]
    fn captions_grow_and_a_paused_sentence_ends_as_one_turn() {
        let mut t = SttTurn::new();
        assert_eq!(
            t.apply(&partial("remind me to call", false, false)),
            TurnStep::Caption("remind me to call".into())
        );
        // A pause to think: Smart Turn demotes it to a chunk final.
        assert_eq!(
            t.apply(&partial("Remind me to call", true, false)),
            TurnStep::Caption("Remind me to call".into())
        );
        assert_eq!(
            t.apply(&partial("the dentist", false, false)),
            TurnStep::Caption("Remind me to call the dentist".into())
        );
        assert_eq!(
            t.apply(&partial(
                "Remind me to call the dentist tomorrow.",
                true,
                true
            )),
            TurnStep::Ended("Remind me to call the dentist tomorrow.".into())
        );
    }

    #[test]
    fn locked_words_left_out_of_the_final_are_kept() {
        let mut t = SttTurn::new();
        t.apply(&partial("My number is five five five", true, false));
        assert_eq!(
            t.apply(&partial("one two three four.", true, true)),
            TurnStep::Ended("My number is five five five one two three four.".into())
        );
    }

    #[test]
    fn noise_alone_ends_with_no_words() {
        let mut t = SttTurn::new();
        assert_eq!(t.apply(&partial("", false, false)), TurnStep::Nothing);
        assert_eq!(
            t.apply(&partial("", true, true)),
            TurnStep::Ended(String::new())
        );
        assert_eq!(t.apply(&SttEvent::Other), TurnStep::Nothing);
    }
}
